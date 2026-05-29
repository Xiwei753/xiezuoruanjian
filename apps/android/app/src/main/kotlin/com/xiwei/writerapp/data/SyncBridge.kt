package com.xiwei.writerapp.data

import com.xiwei.writerapp.model.SyncConfig
import com.xiwei.writerapp.model.SyncDiagnosticsResult
import com.xiwei.writerapp.model.SyncPlan
import com.xiwei.writerapp.model.SyncResult
import com.xiwei.writerapp.model.SyncSecrets
import com.xiwei.writerapp.model.SyncState

class SyncBridge internal constructor(private val nativeBridge: NativeCoreBridge) {
    fun loadSyncState(): BridgeResult<SyncState> = nativeBridge.loadSyncState().toBridgeResult()
    fun loadSyncConfig(): BridgeResult<SyncConfig> = nativeBridge.loadSyncConfig().toBridgeResult()
    fun saveSyncConfig(config: SyncConfig): BridgeResult<Boolean> = nativeBridge.saveSyncConfig(config).toBridgeResult()
    fun loadSyncSecrets(): BridgeResult<SyncSecrets> = nativeBridge.loadSyncSecrets().toBridgeResult()
    fun saveSyncSecrets(secrets: SyncSecrets): BridgeResult<Boolean> = nativeBridge.saveSyncSecrets(secrets).toBridgeResult()
    fun performSyncDiagnostics(config: SyncConfig): BridgeResult<SyncDiagnosticsResult> =
        nativeBridge.performSyncDiagnostics(config).toBridgeResult()
    fun performSyncDryRun(config: SyncConfig): BridgeResult<SyncPlan> =
        nativeBridge.performSyncDryRun(config).toBridgeResult()
    fun performSync(config: SyncConfig): BridgeResult<SyncResult> = nativeBridge.performSync(config).toBridgeResult()
}
