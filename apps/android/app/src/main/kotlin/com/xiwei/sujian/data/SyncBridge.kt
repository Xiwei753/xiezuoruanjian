package com.xiwei.sujian.data

import com.xiwei.sujian.model.SyncCapabilityData
import com.xiwei.sujian.model.SyncConfig
import com.xiwei.sujian.model.SyncDiagnosticsResult
import com.xiwei.sujian.model.SyncPlan
import com.xiwei.sujian.model.SyncResult
import com.xiwei.sujian.model.SyncSecrets
import com.xiwei.sujian.model.SyncState

/**
 * Android 端同步功能桥接层 — 委托 Core `WriterAppService` 的同步方法。
 *
 * 所有方法通过 [WriterAppServiceHolder] 访问 UniFFI 生成的 Core 服务，
 * 返回 [BridgeResult] 封装成功/失败。调用方必须在 UI 线程执行。
 *
 * ## 线程约束
 *
 * UniFFI 调用是同步阻塞的，底层 Core 使用 `Mutex` 保护共享状态。
 * 不得在持有 Android 锁的同时调用此桥接方法，避免与 Core 侧 Mutex 死锁。
 */
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

    // #592 五/六/#595 十：进程级 override（操作作用域凭据）与按 generation 保存凭据。
    fun setSyncSecretsOverride(secrets: SyncSecrets): BridgeResult<Unit> = holder.wrapResult {
        holder.service.setSyncSecretsOverride(secrets.toDto())
    }

    /** #595 十：操作结束后清除进程级 override（Core 侧置 None）。 */
    fun clearSyncSecretsOverride(): BridgeResult<Unit> = holder.wrapResult {
        holder.service.clearSyncSecretsOverride()
    }

    fun saveSyncSecretsForGeneration(generation: ULong, secrets: SyncSecrets): BridgeResult<Boolean> = holder.wrapResult {
        holder.service.saveSyncSecretsForGeneration(generation, secrets.toDto())
    }

    fun loadSyncSecretsForGeneration(generation: ULong): BridgeResult<SyncSecrets?> = holder.wrapResult {
        holder.service.loadSyncSecretsForGeneration(generation)?.toModel()
    }

    /** #595 五：删除指定 generation 的安全存储凭据（旧版本清理）。 */
    fun deleteSyncSecretsForGeneration(generation: ULong): BridgeResult<Unit> = holder.wrapResult {
        holder.service.deleteSyncSecretsForGeneration(generation)
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
