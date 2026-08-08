package com.xiwei.sujian.core.interop.sync
import com.xiwei.sujian.core.interop.app.WriterAppServiceHolder
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.core.interop.common.toDto
import com.xiwei.sujian.core.interop.common.toModel
import com.xiwei.sujian.feature.sync.data.model.SyncCapabilityData
import com.xiwei.sujian.feature.sync.data.model.SyncConfig
import com.xiwei.sujian.feature.sync.data.model.SyncDiagnosticsResult
import com.xiwei.sujian.feature.sync.data.model.SyncPlan
import com.xiwei.sujian.feature.sync.data.model.SyncResult
import com.xiwei.sujian.feature.sync.data.model.SyncSecrets
import com.xiwei.sujian.feature.sync.data.model.SyncState

/**
 * Android 端同步功能桥接层 — 委托 Core `WriterAppService` 的同步方法。
 *
 * #600 评论 #3 问题二/四：作品级 sync config/secrets 入口全部按 projectId 路由；
 * 应用级同步通道（设置/全局星图/主题调色板）走独立入口，无 projectId。
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
    // ── 作品级同步入口（按 projectId 路由） ──

    fun loadSyncConfig(projectId: String): BridgeResult<SyncConfig> =
        holder.wrapResult {
            holder.service.loadSyncConfig(projectId).toModel()
        }

    fun saveSyncConfig(
        projectId: String,
        config: SyncConfig,
    ): BridgeResult<Boolean> =
        holder.wrapResult {
            holder.service.saveSyncConfig(projectId, config.toDto())
        }

    fun loadSyncSecrets(projectId: String): BridgeResult<SyncSecrets> =
        holder.wrapResult {
            holder.service.loadSyncSecrets(projectId).toModel()
        }

    fun saveSyncSecrets(
        projectId: String,
        secrets: SyncSecrets,
    ): BridgeResult<Boolean> =
        holder.wrapResult {
            holder.service.saveSyncSecrets(projectId, secrets.toDto())
        }

    // #592 五/六/#595 十：进程级 override（操作作用域凭据）与按 generation 保存凭据。
    // override 是进程级，不按作品区分（Core 侧单一槽）。
    fun setSyncSecretsOverride(secrets: SyncSecrets): BridgeResult<Unit> =
        holder.wrapResult {
            holder.service.setSyncSecretsOverride(secrets.toDto())
        }

    /** #595 十：操作结束后清除进程级 override（Core 侧置 None）。 */
    fun clearSyncSecretsOverride(): BridgeResult<Unit> =
        holder.wrapResult {
            holder.service.clearSyncSecretsOverride()
        }

    fun saveSyncSecretsForGeneration(
        projectId: String,
        generation: ULong,
        secrets: SyncSecrets,
    ): BridgeResult<Boolean> =
        holder.wrapResult {
            holder.service.saveSyncSecretsForGeneration(projectId, generation, secrets.toDto())
        }

    fun loadSyncSecretsForGeneration(
        projectId: String,
        generation: ULong,
    ): BridgeResult<SyncSecrets?> =
        holder.wrapResult {
            holder.service.loadSyncSecretsForGeneration(projectId, generation)?.toModel()
        }

    /** #595 五：删除指定 generation 的安全存储凭据（旧版本清理）。 */
    fun deleteSyncSecretsForGeneration(
        projectId: String,
        generation: ULong,
    ): BridgeResult<Unit> =
        holder.wrapResult {
            holder.service.deleteSyncSecretsForGeneration(projectId, generation)
        }

    fun loadSyncState(projectId: String): BridgeResult<SyncState> =
        holder.wrapResult {
            holder.service.loadSyncState(projectId).toModel()
        }

    fun getSyncCapability(projectId: String): BridgeResult<SyncCapabilityData> =
        holder.wrapResult {
            val dto = holder.service.getSyncCapability(projectId)
            SyncCapabilityData(
                canRun = dto.canRun,
                blockReasonCode = dto.blockReasonCode,
                blockMessageKey = dto.blockMessageKey,
                messageArgs = dto.messageArgs,
            )
        }

    fun performSyncDiagnostics(
        projectId: String,
        config: SyncConfig,
    ): BridgeResult<SyncDiagnosticsResult> =
        holder.wrapResult {
            holder.service.performSyncDiagnostics(projectId, config.toDto()).toModel()
        }

    fun performSyncDryRun(
        projectId: String,
        config: SyncConfig,
    ): BridgeResult<SyncPlan> =
        holder.wrapResult {
            holder.service.performSyncDryRun(projectId, config.toDto()).toModel()
        }

    fun performSync(
        projectId: String,
        config: SyncConfig,
        forceSync: Boolean = false,
    ): BridgeResult<SyncResult> =
        holder.wrapResult {
            holder.service.performSync(projectId, config.toDto(), forceSync).toModel()
        }

    // ── 应用级同步通道（Issue #600 评论 #3 问题四：设置/全局星图/主题调色板） ──

    fun loadAppSyncConfig(): BridgeResult<SyncConfig> =
        holder.wrapResult {
            holder.service.loadAppSyncConfig().toModel()
        }

    fun saveAppSyncConfig(config: SyncConfig): BridgeResult<Boolean> =
        holder.wrapResult {
            holder.service.saveAppSyncConfig(config.toDto())
        }

    fun loadAppSyncSecrets(): BridgeResult<SyncSecrets> =
        holder.wrapResult {
            holder.service.loadAppSyncSecrets().toModel()
        }

    fun saveAppSyncSecrets(secrets: SyncSecrets): BridgeResult<Boolean> =
        holder.wrapResult {
            holder.service.saveAppSyncSecrets(secrets.toDto())
        }

    fun saveAppSyncSecretsForGeneration(
        generation: ULong,
        secrets: SyncSecrets,
    ): BridgeResult<Boolean> =
        holder.wrapResult {
            holder.service.saveAppSyncSecretsForGeneration(generation, secrets.toDto())
        }

    fun loadAppSyncSecretsForGeneration(generation: ULong): BridgeResult<SyncSecrets?> =
        holder.wrapResult {
            holder.service.loadAppSyncSecretsForGeneration(generation)?.toModel()
        }

    fun deleteAppSyncSecretsForGeneration(generation: ULong): BridgeResult<Unit> =
        holder.wrapResult {
            holder.service.deleteAppSyncSecretsForGeneration(generation)
        }

    fun performAppSyncDiagnostics(config: SyncConfig): BridgeResult<SyncDiagnosticsResult> =
        holder.wrapResult {
            holder.service.performAppSyncDiagnostics(config.toDto()).toModel()
        }

    fun performAppSyncDryRun(config: SyncConfig): BridgeResult<SyncPlan> =
        holder.wrapResult {
            holder.service.performAppSyncDryRun(config.toDto()).toModel()
        }

    fun performAppSync(
        config: SyncConfig,
        forceSync: Boolean = false,
    ): BridgeResult<SyncResult> =
        holder.wrapResult {
            holder.service.performAppSync(config.toDto(), forceSync).toModel()
        }

    // ── 应用级同步状态（Issue #600 评论 #5） ──
    // 路径：<app_data_root>/app-meta/sync/state.local.json

    fun loadAppSyncState(): BridgeResult<SyncState> =
        holder.wrapResult {
            holder.service.loadAppSyncState().toModel()
        }

    fun saveAppSyncState(state: SyncState): BridgeResult<Unit> =
        holder.wrapResult {
            holder.service.saveAppSyncState(state.toDto())
        }
}
