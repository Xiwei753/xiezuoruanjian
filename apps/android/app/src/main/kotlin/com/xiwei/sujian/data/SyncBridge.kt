package com.xiwei.sujian.data

import com.xiwei.sujian.model.SyncCapabilityData
import com.xiwei.sujian.model.SyncConfig
import com.xiwei.sujian.model.SyncDiagnosticsResult
import com.xiwei.sujian.model.SyncPlan
import com.xiwei.sujian.model.SyncResult
import com.xiwei.sujian.model.SyncSecrets
import com.xiwei.sujian.model.SyncState

class SyncBridge internal constructor(private val holder: WriterAppServiceHolder) {
    fun loadSyncConfig(): BridgeResult<SyncConfig> = holder.wrapResult {
        holder.service.loadSyncConfig().toModel()
    }

    fun saveSyncConfig(config: SyncConfig): BridgeResult<Boolean> = holder.wrapResult {
        holder.service.saveSyncConfig(config.toDto())
    }

    fun loadSyncSecrets(): BridgeResult<SyncSecrets> = holder.wrapResult {
        holder.service.loadSyncSecrets().toModel()
    }

    fun saveSyncSecrets(secrets: SyncSecrets): BridgeResult<Boolean> = holder.wrapResult {
        holder.service.saveSyncSecrets(secrets.toDto())
    }

    fun loadSyncState(): BridgeResult<SyncState> = holder.wrapResult {
        holder.service.loadSyncState().toModel()
    }

    fun getSyncCapability(): BridgeResult<SyncCapabilityData> = holder.wrapResult {
        val dto = holder.service.getSyncCapability()
        SyncCapabilityData(
            canRun = dto.canRun,
            blockReasonCode = dto.blockReasonCode,
            blockMessageKey = dto.blockMessageKey,
            messageArgs = dto.messageArgs
        )
    }

    fun performSyncDiagnostics(config: SyncConfig): BridgeResult<SyncDiagnosticsResult> = holder.wrapResult {
        holder.service.performSyncDiagnostics(config.toDto()).toModel()
    }

    fun performSyncDryRun(config: SyncConfig): BridgeResult<SyncPlan> = holder.wrapResult {
        holder.service.performSyncDryRun(config.toDto()).toModel()
    }

    fun performSync(config: SyncConfig, forceSync: Boolean = false): BridgeResult<SyncResult> = holder.wrapResult {
        holder.service.performSync(config.toDto(), forceSync).toModel()
    }
}
