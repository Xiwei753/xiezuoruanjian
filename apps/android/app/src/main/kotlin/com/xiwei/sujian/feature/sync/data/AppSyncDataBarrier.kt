package com.xiwei.sujian.feature.sync.data
import com.xiwei.sujian.core.diagnostics.DiagnosticsLogger
import com.xiwei.sujian.core.interop.common.BridgeResult
import com.xiwei.sujian.feature.starmap.data.interop.StarMapBridge
import com.xiwei.sujian.feature.sync.data.model.FullSyncResult
import com.xiwei.sujian.feature.sync.data.model.SyncResult

/**
 * #600 评论 #5: 应用级同步数据屏障 - 在应用级同步前后保护本地缓存一致性.
 *
 * - [flushBeforeSync]: 同步开始前 flush 所有星图 store, 确保本地星图写入落盘,
 *   避免同步引擎读到内存中未持久化的星图数据.
 * - [reloadAfterSync]: 同步成功后根据下载文件列表失效对应缓存:
 *   - starmaps/ prefix -> 失效星图缓存
 *   - settings.sync.json -> 重载设置
 *   - themes/palettes/ prefix -> 重载主题调色板
 *
 * 屏障通过 lambda 注入重载回调, 不直接依赖具体 Repository/Store,
 * 便于测试和避免循环依赖.
 */
open class AppSyncDataBarrier(
    private val starmapBridge: StarMapBridge,
    private val reloadSettings: suspend () -> Unit,
    private val reloadThemes: suspend () -> Unit,
    private val invalidateStarmapCache: suspend () -> Unit,
) {
    /**
     * 同步前 flush 所有星图 store. 返回 false 表示 flush 失败, 调用方应终止同步.
     * NotLoaded (原生库未加载) 视为成功 - 同步本身也会以 NotLoaded 失败,
     * 此处不提前阻断.
     *
     * 标记为 [open] 供单元测试 fake（注入 flush 失败验证 SyncCoordinator
     * 的预处理失败路径写 Core FullSyncState）。
     */
    open fun flushBeforeSync(): Boolean {
        return when (val result = starmapBridge.flushAllStarmapStores()) {
            is BridgeResult.Success -> true
            is BridgeResult.Error -> {
                DiagnosticsLogger.w(TAG, "flushAllStarmapStores failed: ${result.fullEnvelope}")
                false
            }
            BridgeResult.NotLoaded -> true
        }
    }

    /**
     * 同步后根据下载/删除文件列表失效对应缓存. 每个分支独立 try-catch,
     * 单个缓存重载失败不影响其他缓存.
     *
     * #600 评论 #7: 除 downloadedFiles 外也检查 localDeletes/remoteDeletes —
     * Git pull 可能删除文件（远端删除），删除一个 starmap 或 palette 文件
     * 同样必须清缓存。
     */
    suspend fun reloadAfterSync(result: SyncResult) {
        val downloaded = result.downloadedFiles
        val deleted = result.localDeletes + result.remoteDeletes
        val allChanged = downloaded + deleted
        if (allChanged.any { it.startsWith("starmaps/") }) {
            try {
                invalidateStarmapCache()
            } catch (e: Exception) {
                DiagnosticsLogger.w(TAG, "invalidate starmap cache failed", e)
            }
        }
        if (allChanged.any { it == "settings.sync.json" }) {
            try {
                reloadSettings()
            } catch (e: Exception) {
                DiagnosticsLogger.w(TAG, "reload settings failed", e)
            }
        }
        if (allChanged.any { it.startsWith("themes/palettes/") }) {
            try {
                reloadThemes()
            } catch (e: Exception) {
                DiagnosticsLogger.w(TAG, "reload themes failed", e)
            }
        }
    }

    /**
     * #630 评论 #1：全量同步成功后根据聚合结果失效对应缓存。
     *
     * 遍历所有 target 的下载/删除文件列表，按前缀触发对应缓存重载。
     * 单个缓存重载失败不影响其他缓存。
     */
    suspend fun reloadAfterFullSync(result: FullSyncResult) {
        val allChanged = mutableListOf<String>()
        for (target in result.targets) {
            allChanged += target.result.downloadedFiles
            allChanged += target.result.localDeletes
            allChanged += target.result.remoteDeletes
        }
        if (allChanged.any { it.startsWith("starmaps/") }) {
            try {
                invalidateStarmapCache()
            } catch (e: Exception) {
                DiagnosticsLogger.w(TAG, "invalidate starmap cache failed", e)
            }
        }
        if (allChanged.any { it == "settings.sync.json" }) {
            try {
                reloadSettings()
            } catch (e: Exception) {
                DiagnosticsLogger.w(TAG, "reload settings failed", e)
            }
        }
        if (allChanged.any { it.startsWith("themes/palettes/") }) {
            try {
                reloadThemes()
            } catch (e: Exception) {
                DiagnosticsLogger.w(TAG, "reload themes failed", e)
            }
        }
    }

    companion object {
        private const val TAG = "AppSyncDataBarrier"
    }
}
