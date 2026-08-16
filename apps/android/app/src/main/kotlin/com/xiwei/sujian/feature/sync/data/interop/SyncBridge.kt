package com.xiwei.sujian.feature.sync.data.interop
import com.xiwei.sujian.core.interop.app.WriterAppServiceHolder
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.core.interop.common.toDto
import com.xiwei.sujian.core.interop.common.toModel
import com.xiwei.sujian.feature.sync.data.model.FullSyncDiagnosticsResult
import com.xiwei.sujian.feature.sync.data.model.FullSyncDryRunResult
import com.xiwei.sujian.feature.sync.data.model.FullSyncResult
import com.xiwei.sujian.feature.sync.data.model.SyncCapabilityData
import com.xiwei.sujian.feature.sync.data.model.SyncConfig
import com.xiwei.sujian.feature.sync.data.model.SyncSecrets
import com.xiwei.sujian.feature.sync.data.model.SyncState

/**
 * Android 端同步功能桥接层 — 委托 Core `WriterAppService` 的同步方法。
 *
 * #630 评论 #1：全量同步统一入口。全应用只存在一份全局 SyncConfig / SyncSecrets，
 * 不再按 projectId 路由 config/secrets。App / Project 仅作为 Core 内部 target，
 * 远端按 `app/` 与 `projects/<project_id>/` 前缀分流；Android 不再自己循环调用
 * 两套 API，直接消费 Core 返回的 [FullSyncResult] 聚合结果。
 *
 * per-target 保留的 API（仍带 projectId 或独立 app 入口）：
 * - [loadSyncState]：读取某个 Project target 的本地 state.local.json；
 * - [loadAppSyncState] / [saveAppSyncState]：读取/写入 App target 的本地 state。
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
    // ── 全局同步配置 / 凭据 ──

    fun loadSyncConfig(): BridgeResult<SyncConfig> =
        holder.wrapResult {
            holder.service.loadSyncConfig().toModel()
        }

    fun saveSyncConfig(config: SyncConfig): BridgeResult<Boolean> =
        holder.wrapResult {
            holder.service.saveSyncConfig(config.toDto())
        }

    fun loadSyncSecrets(): BridgeResult<SyncSecrets> =
        holder.wrapResult {
            holder.service.loadSyncSecrets().toModel()
        }

    fun saveSyncSecrets(secrets: SyncSecrets): BridgeResult<Boolean> =
        holder.wrapResult {
            holder.service.saveSyncSecrets(secrets.toDto())
        }

    // #592 五/六/#595 十：进程级 override（操作作用域凭据）与按 generation 保存凭据。
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
        generation: ULong,
        secrets: SyncSecrets,
    ): BridgeResult<Boolean> =
        holder.wrapResult {
            holder.service.saveSyncSecretsForGeneration(generation, secrets.toDto())
        }

    fun loadSyncSecretsForGeneration(generation: ULong): BridgeResult<SyncSecrets?> =
        holder.wrapResult {
            holder.service.loadSyncSecretsForGeneration(generation)?.toModel()
        }

    /** #595 五：删除指定 generation 的安全存储凭据（旧版本清理）。 */
    fun deleteSyncSecretsForGeneration(generation: ULong): BridgeResult<Unit> =
        holder.wrapResult {
            holder.service.deleteSyncSecretsForGeneration(generation)
        }

    // ── 全局同步能力 ──

    fun getSyncCapability(): BridgeResult<SyncCapabilityData> =
        holder.wrapResult {
            val dto = holder.service.getSyncCapability()
            SyncCapabilityData(
                canRun = dto.canRun,
                blockReasonCode = dto.blockReasonCode,
                blockMessageKey = dto.blockMessageKey,
                messageArgs = dto.messageArgs,
            )
        }

    // ── 全量同步执行入口（Issue #630 评论 #1） ──

    fun performFullSync(
        config: SyncConfig,
        forceSync: Boolean = false,
    ): BridgeResult<FullSyncResult> =
        holder.wrapResult {
            holder.service.performFullSync(config.toDto(), forceSync).toModel()
        }

    fun performFullSyncDryRun(config: SyncConfig): BridgeResult<FullSyncDryRunResult> =
        holder.wrapResult {
            holder.service.performFullSyncDryRun(config.toDto()).toModel()
        }

    fun performFullSyncDiagnostics(config: SyncConfig): BridgeResult<FullSyncDiagnosticsResult> =
        holder.wrapResult {
            holder.service.performFullSyncDiagnostics(config.toDto()).toModel()
        }

    // ── per-target 同步状态查询（App / Project 各自的本地 state） ──
    // App: <app_data_root>/app-meta/sync/state.local.json
    // Project: <project_root>/app-meta/sync/state.local.json

    fun loadSyncState(projectId: String): BridgeResult<SyncState> =
        holder.wrapResult {
            holder.service.loadSyncState(projectId).toModel()
        }

    fun loadAppSyncState(): BridgeResult<SyncState> =
        holder.wrapResult {
            holder.service.loadAppSyncState().toModel()
        }

    fun saveAppSyncState(state: SyncState): BridgeResult<Unit> =
        holder.wrapResult {
            holder.service.saveAppSyncState(state.toDto())
        }
}
